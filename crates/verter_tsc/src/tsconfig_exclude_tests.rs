//! tsconfig `exclude` selection follows TypeScript's matcher: an entry removes
//! only the files it matches, anchored at the tsconfig directory.

use super::*;

/// Files every fixture project carries, relative to the project root.
const FIXTURE_FILES: &[&str] = &[
    "src/Bad.vue",
    "src/dist/Built.vue",
    "src/Card.spec.ts",
    "src/node_modules/dep/Dep.vue",
    "src/bower_components/dep/Bower.vue",
    "app.config.ts",
    "other/x.ts",
];

/// Write the fixture tree under `root` and a tsconfig at `root/<tsconfig_dir>`.
fn write_project(root: &Path, tsconfig_dir: &str, include: &[&str], exclude: &[&str]) -> PathBuf {
    for file in FIXTURE_FILES {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = if file.ends_with(".vue") {
            "<script setup lang=\"ts\">\nconst count: number = \"not a number\";\n</script>\n"
        } else {
            "export {};\n"
        };
        std::fs::write(path, body).unwrap();
    }
    let config_dir = root.join(tsconfig_dir);
    std::fs::create_dir_all(&config_dir).unwrap();
    let tsconfig = config_dir.join("tsconfig.json");
    let config = serde_json::json!({ "include": include, "exclude": exclude });
    std::fs::write(&tsconfig, config.to_string()).unwrap();
    tsconfig
}

/// The selected `.vue` and `.ts` files, relative to `root`, sorted.
fn selection(root: &Path, tsconfig: &Path) -> Vec<String> {
    let root = root.canonicalize().unwrap();
    let root = simplify_verbatim_path(&root).into_owned();
    let config = load_tsconfig(tsconfig).unwrap();
    let mut names: Vec<String> = config
        .vue_files
        .iter()
        .chain(&config.ts_files)
        .map(|p| {
            p.strip_prefix(&root)
                .unwrap_or_else(|_| panic!("{} outside {}", p.display(), root.display()))
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    names.sort();
    names
}

const EVERYTHING: &[&str] = &["src/Bad.vue", "src/Card.spec.ts", "src/dist/Built.vue"];

struct Row {
    tsconfig_dir: &'static str,
    include: &'static str,
    exclude: &'static [&'static str],
    selected: &'static [&'static str],
}

const fn row(
    tsconfig_dir: &'static str,
    include: &'static str,
    exclude: &'static [&'static str],
    selected: &'static [&'static str],
) -> Row {
    Row {
        tsconfig_dir,
        include,
        exclude,
        selected,
    }
}

const ROWS: &[Row] = &[
    // No exclude: package folders are never walked.
    row("", "src/**/*", &[], EVERYTHING),
    // Entries that match no included file remove nothing.
    row("", "src/**/*", &["other/**/*"], EVERYTHING),
    row("", "src/**/*", &["*.ts"], EVERYTHING),
    row("", "src/**/*", &["app.config.*"], EVERYTHING),
    row("", "src/**/*", &["other/*.ts"], EVERYTHING),
    row("", "src/**/*", &["other"], EVERYTHING),
    row(
        "",
        "src/**/*",
        &[
            "nuxt.config.*",
            "modules/*/runtime/server/**/*",
            "layers/*/server/**/*",
        ],
        EVERYTHING,
    ),
    // Entries that match remove exactly what they match.
    row(
        "",
        "src/**/*",
        &["src/Bad.vue"],
        &["src/Card.spec.ts", "src/dist/Built.vue"],
    ),
    row("", "src/**/*", &["src/**/*.vue"], &["src/Card.spec.ts"]),
    row("", "src/**/*", &["src"], &[]),
    row(
        "",
        "src/**/*",
        &["src/B?d.vue", "src/*/*"],
        &["src/Card.spec.ts"],
    ),
    row(
        "",
        "src/**/*",
        &["**/dist"],
        &["src/Bad.vue", "src/Card.spec.ts"],
    ),
    row(
        "",
        "src/**/*",
        &["**/*.spec.ts"],
        &["src/Bad.vue", "src/dist/Built.vue"],
    ),
    row(
        "",
        "src/**/*",
        &["src/**/*.spec.ts"],
        &["src/Bad.vue", "src/dist/Built.vue"],
    ),
    row(
        "",
        "src/**/*",
        &["src/dist"],
        &["src/Bad.vue", "src/Card.spec.ts"],
    ),
    // A tsconfig in a subdirectory anchors `..` entries at its own directory.
    row("sub", "../src/**/*", &[], EVERYTHING),
    row("sub", "../src/**/*", &["../*.ts"], EVERYTHING),
    row("sub", "../src/**/*", &["../app.config.*"], EVERYTHING),
    row("sub", "../src/**/*", &["../other/**/*"], EVERYTHING),
    row("sub", "../src/**/*", &["../other/*.ts"], EVERYTHING),
    row("sub", "../src/**/*", &["../other/*/x.ts"], EVERYTHING),
    row(
        ".nuxt",
        "../src/**/*",
        &[
            "../nuxt.config.*",
            "../modules/*/runtime/server/**/*",
            "../layers/*/server/**/*",
        ],
        EVERYTHING,
    ),
    row(
        "sub",
        "../src/**/*",
        &["../src/Bad.vue"],
        &["src/Card.spec.ts", "src/dist/Built.vue"],
    ),
    row("sub", "../src/**/*", &["../src"], &[]),
    // An entry is not anchored at the tsconfig's own directory name.
    row("sub", "../src/**/*", &["src/**/*"], EVERYTHING),
    // `..` after `**` is an invalid TypeScript spec and is dropped.
    row("", "src/**/*", &["**/../src/**/*"], EVERYTHING),
];

#[test]
fn exclude_removes_only_the_files_it_matches() {
    let mut failures = Vec::new();
    for row in ROWS {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("ws");
        let tsconfig = write_project(&root, row.tsconfig_dir, &[row.include], row.exclude);
        let actual = selection(&root, &tsconfig);
        if actual != row.selected {
            failures.push(format!(
                "tsconfig `{}/tsconfig.json`, include {:?}, exclude {:?}: selected {actual:?}, \
                 expected {:?}",
                row.tsconfig_dir, row.include, row.exclude, row.selected
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A `**/dist` or `node_modules` ancestor of the project root is not part of
/// the tsconfig-relative path the entries are matched against.
#[test]
fn exclude_ignores_matching_ancestors_of_the_project_root() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().join("dist").join("node_modules").join("app");
    let tsconfig = write_project(&root, "", &["src/**/*"], &["**/dist"]);
    assert_eq!(
        selection(&root, &tsconfig),
        ["src/Bad.vue", "src/Card.spec.ts"]
    );
}

/// A package folder spelled literally by `include` is walked; wildcards never
/// descend into one.
#[test]
fn package_folders_are_walked_only_when_include_names_them() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().join("ws");
    let tsconfig = write_project(&root, "", &["src/node_modules/dep/*"], &[]);
    assert_eq!(
        selection(&root, &tsconfig),
        ["src/node_modules/dep/Dep.vue"]
    );
}

#[test]
fn include_matched_carriers_counts_what_exclude_removed() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().join("ws");
    let tsconfig = write_project(&root, "", &["src/**/*"], &["src"]);
    assert!(load_tsconfig(&tsconfig).unwrap().vue_files.is_empty());
    assert_eq!(include_matched_carrier_count(&tsconfig), 2);

    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().join("ws");
    let tsconfig = write_project(&root, "", &["other/**/*"], &[]);
    assert_eq!(include_matched_carrier_count(&tsconfig), 0);
}

#[test]
fn exclude_entries_follow_platform_case_sensitivity() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().join("ws");
    let tsconfig = write_project(&root, "", &["src/**/*"], &["SRC/BAD.VUE"]);
    let selected = selection(&root, &tsconfig);
    let dropped = !selected.iter().any(|f| f == "src/Bad.vue");
    assert_eq!(dropped, !FILE_NAMES_ARE_CASE_SENSITIVE, "{selected:?}");
}
