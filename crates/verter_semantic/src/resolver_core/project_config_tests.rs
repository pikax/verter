use super::*;
use crate::resolver_core::membership::StaticMembershipSpec;
use rustc_hash::FxHashSet;

fn empty_membership() -> ConfiguredMembership {
    ConfiguredMembership {
        spec: StaticMembershipSpec {
            files: Vec::new(),
            include: Vec::new(),
            exclude: Vec::new().into(),
        },
        materialized_files: FxHashSet::default(),
    }
}

fn config(root: &str, membership: ConfiguredMembership) -> IdeProjectConfig {
    IdeProjectConfig {
        root: root.to_string(),
        workspace_root: root.to_string(),
        tsconfig_path: Some(format!("{root}/tsconfig.json")),
        provider_root: root.to_string(),
        workspace_aliases: Vec::new(),
        compiler_options: IdeProjectCompilerOptions::default(),
        references: Vec::new(),
        membership,
    }
}

#[test]
fn compiler_options_js_is_member_true_on_allow_js() {
    let mut options = IdeProjectCompilerOptions::default();
    assert!(!options.js_is_member());
    options.allow_js = true;
    assert!(options.js_is_member());
}

#[test]
fn compiler_options_js_is_member_true_on_check_js() {
    let options = IdeProjectCompilerOptions {
        check_js: true,
        ..IdeProjectCompilerOptions::default()
    };
    assert!(options.js_is_member());
}

#[test]
fn matches_file_delegates_to_membership_contains() {
    let mut materialized = FxHashSet::default();
    materialized.insert(CanonicalPath::new("/proj/src/main.ts"));
    let membership = ConfiguredMembership {
        spec: StaticMembershipSpec {
            files: Vec::new(),
            include: Vec::new(),
            exclude: Vec::new().into(),
        },
        materialized_files: materialized,
    };
    let cfg = config("/proj", membership);
    assert!(cfg.matches_file("/proj/src/main.ts"));
    assert!(!cfg.matches_file("/proj/src/other.ts"));
}

#[test]
fn matches_file_false_for_empty_membership() {
    let cfg = config("/proj", empty_membership());
    assert!(!cfg.matches_file("/proj/src/main.ts"));
}

// ── Effective semantic compiler options ──

/// TypeScript 7.0.2 defaults, pinned against the shipped `tsc.exe` (see the
/// `SemanticCompilerOptions::default` doc for the probe): the whole
/// `strict` family ON, exact optionality / unchecked indexed access / noLib
/// OFF, no explicit lib, default target loading `lib.es2025.full.d.ts`.
#[test]
fn semantic_compiler_options_default_pins_typescript_defaults() {
    let defaults = SemanticCompilerOptions::default();
    assert!(defaults.strict_null_checks);
    assert!(defaults.strict_function_types);
    assert!(defaults.strict_bind_call_apply);
    assert!(defaults.strict_property_initialization);
    assert!(defaults.no_implicit_any);
    assert!(defaults.no_implicit_this);
    assert!(defaults.use_unknown_in_catch_variables);
    assert!(defaults.always_strict);
    assert!(!defaults.exact_optional_property_types);
    assert!(!defaults.no_unchecked_indexed_access);
    assert!(!defaults.no_lib);
    assert_eq!(defaults.lib, None);
    assert_eq!(defaults.target, ScriptTarget::Es2025);
    assert_eq!(
        defaults.effective_lib_file_names(),
        vec!["lib.es2025.full.d.ts"]
    );
    assert_eq!(
        RawSemanticCompilerOptions::default().effective(),
        defaults,
        "a config declaring nothing runs on the TypeScript defaults"
    );
}

/// An explicit umbrella `strict: false` switches every undeclared member
/// off, while a member declared beside it keeps its own value — and a
/// leaf's declaration wins over an inherited one.
#[test]
fn strict_umbrella_expands_to_undeclared_members_only() {
    let mut base = RawSemanticCompilerOptions {
        strict: Some(false),
        ..Default::default()
    };
    let relaxed = base.effective();
    assert!(!relaxed.strict_null_checks);
    assert!(!relaxed.strict_function_types);
    assert!(!relaxed.no_implicit_any);
    assert!(!relaxed.use_unknown_in_catch_variables);
    assert!(
        !relaxed.exact_optional_property_types && !relaxed.no_unchecked_indexed_access,
        "non-umbrella options are untouched by `strict`"
    );

    base.layer(RawSemanticCompilerOptions {
        strict_null_checks: Some(true),
        ..Default::default()
    });
    let mixed = base.effective();
    assert!(
        mixed.strict_null_checks,
        "an explicit member overrides the umbrella"
    );
    assert!(
        !mixed.strict_function_types,
        "the other members still follow the umbrella"
    );

    base.layer(RawSemanticCompilerOptions {
        strict_null_checks: Some(false),
        ..Default::default()
    });
    assert!(
        !base.effective().strict_null_checks,
        "a nearer declaration replaces the inherited member value"
    );
}

/// Different spellings of the same effective configuration are EQUAL —
/// the value the type layer keys on is the effective set, never the text.
#[test]
fn equivalent_spellings_produce_equal_effective_options() {
    let umbrella = RawSemanticCompilerOptions {
        strict: Some(false),
        ..Default::default()
    }
    .effective();
    let spelled_out = RawSemanticCompilerOptions {
        strict_null_checks: Some(false),
        strict_function_types: Some(false),
        strict_bind_call_apply: Some(false),
        strict_property_initialization: Some(false),
        no_implicit_any: Some(false),
        no_implicit_this: Some(false),
        use_unknown_in_catch_variables: Some(false),
        always_strict: Some(false),
        ..Default::default()
    }
    .effective();
    assert_eq!(umbrella, spelled_out);

    let implicit_default = RawSemanticCompilerOptions::default().effective();
    let explicit_default = RawSemanticCompilerOptions {
        strict: Some(true),
        target: Some("ES2025".to_string()),
        ..Default::default()
    }
    .effective();
    assert_eq!(implicit_default, explicit_default);
}

/// `lib` entries canonicalise to the lib FILE names the binary loads, as a
/// deduplicated sorted set; an explicit empty selection stays distinct
/// from an unset one; `noLib` empties the effective selection.
#[test]
fn lib_selection_canonicalises_to_a_sorted_file_name_set() {
    let declared = RawSemanticCompilerOptions {
        lib: Some(vec![
            "DOM".to_string(),
            "ES6".to_string(),
            "dom".to_string(),
            "es7".to_string(),
            "esnext.iterator".to_string(),
        ]),
        ..Default::default()
    }
    .effective();
    assert_eq!(
        declared.lib.as_deref(),
        Some(
            &[
                "lib.dom.d.ts".to_string(),
                "lib.es2015.d.ts".to_string(),
                "lib.es2016.d.ts".to_string(),
                "lib.esnext.iterator.d.ts".to_string(),
            ][..]
        )
    );
    assert_eq!(
        declared.effective_lib_file_names(),
        vec![
            "lib.dom.d.ts",
            "lib.es2015.d.ts",
            "lib.es2016.d.ts",
            "lib.esnext.iterator.d.ts",
        ]
    );
    let reordered = RawSemanticCompilerOptions {
        lib: Some(vec![
            "esnext.iterator".to_string(),
            "es2016".to_string(),
            "es2015".to_string(),
            "dom".to_string(),
        ]),
        ..Default::default()
    }
    .effective();
    assert_eq!(declared, reordered, "declaration order is not semantic");

    let explicit_empty = RawSemanticCompilerOptions {
        lib: Some(Vec::new()),
        ..Default::default()
    }
    .effective();
    assert_eq!(explicit_empty.lib, Some(Vec::new()));
    assert!(explicit_empty.effective_lib_file_names().is_empty());
    assert_ne!(explicit_empty, SemanticCompilerOptions::default());

    let no_lib = RawSemanticCompilerOptions {
        no_lib: Some(true),
        ..Default::default()
    }
    .effective();
    assert!(no_lib.effective_lib_file_names().is_empty());
}

/// `target` selects the default lib file only while `lib` is unset;
/// rejected spellings fall back to the default target.
#[test]
fn target_selects_the_default_lib_file_only_when_lib_is_unset() {
    let es2020 = RawSemanticCompilerOptions {
        target: Some("ES2020".to_string()),
        ..Default::default()
    }
    .effective();
    assert_eq!(es2020.target, ScriptTarget::Es2020);
    assert_eq!(
        es2020.effective_lib_file_names(),
        vec!["lib.es2020.full.d.ts"]
    );
    assert_eq!(
        ScriptTarget::from_config_value("es6"),
        Some(ScriptTarget::Es2015)
    );
    assert_eq!(ScriptTarget::Es2015.default_lib_file_name(), "lib.es6.d.ts");
    assert_eq!(ScriptTarget::Es5.default_lib_file_name(), "lib.d.ts");
    assert_eq!(
        ScriptTarget::EsNext.default_lib_file_name(),
        "lib.esnext.full.d.ts"
    );
    assert_eq!(ScriptTarget::from_config_value("es3"), None);
    assert_eq!(ScriptTarget::from_config_value("es7"), None);
    let rejected = RawSemanticCompilerOptions {
        target: Some("es3".to_string()),
        ..Default::default()
    }
    .effective();
    assert_eq!(rejected.target, ScriptTarget::default());

    let with_lib = RawSemanticCompilerOptions {
        target: Some("es2020".to_string()),
        lib: Some(vec!["dom".to_string()]),
        ..Default::default()
    }
    .effective();
    assert_eq!(
        with_lib.effective_lib_file_names(),
        vec!["lib.dom.d.ts"],
        "an explicit lib selection replaces the target's default lib"
    );
}
