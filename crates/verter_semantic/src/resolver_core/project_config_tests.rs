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
