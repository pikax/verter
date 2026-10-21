//! tsconfig `exclude` selection through the `verter-tsc` binary: an entry that
//! matches no included file leaves the `.vue` selection intact, and an
//! `exclude` that removes every included carrier is reported, not passed
//! silently.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BAD_VUE: &str =
    "<script setup lang=\"ts\">\nconst count: number = \"not a number\";\n</script>\n\n<template>\n  <p>{{ count }}</p>\n</template>\n";

/// `root/src/Bad.vue` plus a tsconfig at `root/<tsconfig_dir>/tsconfig.json`.
fn project(root: &Path, tsconfig_dir: &str, include: &str, exclude: &[&str]) -> PathBuf {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/Bad.vue"), BAD_VUE).unwrap();
    let config_dir = root.join(tsconfig_dir);
    std::fs::create_dir_all(&config_dir).unwrap();
    let tsconfig = config_dir.join("tsconfig.json");
    let config = serde_json::json!({
        "compilerOptions": { "strict": true, "noEmit": true },
        "include": [include],
        "exclude": exclude,
    });
    std::fs::write(&tsconfig, config.to_string()).unwrap();
    tsconfig
}

/// Run the binary hermetically: no engine override and an empty `PATH`.
fn verter_tsc(args: &[&str], tsconfig: &Path) -> Output {
    let empty_path = tempfile::TempDir::new().unwrap();
    Command::new(verter_test_support::cargo_test_binary_path!("verter-tsc"))
        .args(args)
        .arg("-p")
        .arg(tsconfig)
        .env_remove("VERTER_TSGO_BIN")
        .env("PATH", empty_path.path())
        .output()
        .expect("failed to execute verter-tsc")
}

#[test]
fn non_matching_exclude_keeps_the_included_vue_file() {
    let rows: &[(&str, &str, &[&str])] = &[
        ("", "src/**/*", &["other/**/*"]),
        ("", "src/**/*", &["*.ts"]),
        ("", "src/**/*", &["app.config.*"]),
        ("sub", "../src/**/*", &["../*.ts"]),
        ("sub", "../src/**/*", &["../other/**/*"]),
    ];
    for (tsconfig_dir, include, exclude) in rows {
        let temp = tempfile::TempDir::new().unwrap();
        let tsconfig = project(temp.path(), tsconfig_dir, include, exclude);
        let output = verter_tsc(&["--listFiles"], &tsconfig);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{exclude:?}: {stderr}");
        assert!(
            stdout.lines().any(|line| line.ends_with("Bad.vue")),
            "exclude {exclude:?} from `{tsconfig_dir}/tsconfig.json` must keep src/Bad.vue \
             selected:\n{stdout}\n{stderr}"
        );
        assert!(!stderr.contains("warning"), "{exclude:?}: {stderr}");
    }
}

#[test]
fn exclude_removing_every_included_carrier_is_reported() {
    let temp = tempfile::TempDir::new().unwrap();
    let tsconfig = project(temp.path(), "", "src/**/*", &["src/**/*.vue"]);
    let output = verter_tsc(&["--noEmit"], &tsconfig);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("checking 0 .vue file(s)")
            && stderr.contains("`include` matched 1 .vue file(s)")
            && stderr.contains("`exclude` removed all of them"),
        "an exclude that empties the selection must be reported:\n{stderr}"
    );
}

#[test]
fn include_matching_no_carrier_is_not_reported() {
    let temp = tempfile::TempDir::new().unwrap();
    let tsconfig = project(temp.path(), "", "other/**/*", &["src"]);
    let output = verter_tsc(&["--noEmit"], &tsconfig);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
}
