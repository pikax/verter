use super::*;

const SIMPLE_PKG: &str = r#"{
    "name": "my-package",
    "version": "1.0.0",
    "main": "./dist/index.js",
    "module": "./dist/index.mjs",
    "types": "./dist/index.d.ts"
}"#;

const PKG_WITH_EXPORTS: &str = r##"{
    "name": "my-package",
    "version": "2.0.0",
    "exports": {
        ".": {
            "import": "./dist/index.mjs",
            "require": "./dist/index.cjs",
            "types": "./dist/index.d.ts"
        },
        "./utils": "./dist/utils.mjs"
    },
    "imports": {
        "#internal": "./src/internal.ts"
    }
}"##;

#[test]
fn get_or_parse_caches_manifest() {
    let mut index = PackageIndex::new();

    let manifest = index.get_or_parse("node_modules/my-package/package.json", SIMPLE_PKG);
    assert_eq!(manifest.name.as_deref(), Some("my-package"));
    assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
    assert_eq!(manifest.main.as_deref(), Some("./dist/index.js"));
    assert_eq!(manifest.module.as_deref(), Some("./dist/index.mjs"));
    assert_eq!(manifest.types.as_deref(), Some("./dist/index.d.ts"));
    assert!(manifest.typings.is_none());

    // Second call should return cached
    let manifest2 = index.get_or_parse("node_modules/my-package/package.json", "GARBAGE");
    assert_eq!(
        manifest2.name.as_deref(),
        Some("my-package"),
        "should return cached manifest, not re-parse"
    );
}

#[test]
fn get_cached_returns_none_before_parse() {
    let index = PackageIndex::new();
    assert!(index.get_cached("node_modules/foo/package.json").is_none());
}

#[test]
fn get_cached_returns_manifest_after_parse() {
    let mut index = PackageIndex::new();
    index.get_or_parse("node_modules/foo/package.json", SIMPLE_PKG);

    let cached = index.get_cached("node_modules/foo/package.json");
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().name.as_deref(), Some("my-package"));
}

#[test]
fn invalidate_removes_cached_entry() {
    let mut index = PackageIndex::new();
    index.get_or_parse("node_modules/foo/package.json", SIMPLE_PKG);

    assert!(index.invalidate("node_modules/foo/package.json"));
    assert!(index.get_cached("node_modules/foo/package.json").is_none());
    assert!(
        !index.invalidate("node_modules/foo/package.json"),
        "second invalidate should return false"
    );
}

#[test]
fn invalidate_under_removes_matching_entries() {
    let mut index = PackageIndex::new();
    index.get_or_parse("node_modules/foo/package.json", SIMPLE_PKG);
    index.get_or_parse("node_modules/foo/sub/package.json", SIMPLE_PKG);
    index.get_or_parse("node_modules/bar/package.json", SIMPLE_PKG);

    index.invalidate_under("node_modules/foo/");
    assert!(index.get_cached("node_modules/foo/package.json").is_none());
    assert!(index
        .get_cached("node_modules/foo/sub/package.json")
        .is_none());
    assert!(
        index.get_cached("node_modules/bar/package.json").is_some(),
        "bar should not be invalidated"
    );
}

#[test]
fn parses_exports_and_imports() {
    let mut index = PackageIndex::new();
    let manifest = index.get_or_parse("node_modules/pkg/package.json", PKG_WITH_EXPORTS);

    assert!(manifest.exports.is_some());
    assert!(manifest.imports.is_some());

    let exports = manifest.exports.as_ref().unwrap();
    assert!(exports.get(".").is_some());
    assert!(exports.get("./utils").is_some());

    let imports = manifest.imports.as_ref().unwrap();
    assert!(imports.get("#internal").is_some());
}

#[test]
fn invalid_json_returns_default_manifest() {
    let mut index = PackageIndex::new();
    let manifest = index.get_or_parse("node_modules/broken/package.json", "NOT JSON");

    assert!(manifest.name.is_none());
    assert!(manifest.main.is_none());
    assert!(manifest.raw.is_some(), "raw source should be preserved");
}

#[test]
fn non_object_json_returns_default_manifest() {
    let mut index = PackageIndex::new();
    let manifest = index.get_or_parse("node_modules/weird/package.json", "[1, 2, 3]");

    assert!(manifest.name.is_none());
    assert!(manifest.raw.is_some());
}

#[test]
fn len_and_empty() {
    let mut index = PackageIndex::new();
    assert!(index.is_empty());
    assert_eq!(index.len(), 0);

    index.get_or_parse("a/package.json", SIMPLE_PKG);
    assert!(!index.is_empty());
    assert_eq!(index.len(), 1);

    index.get_or_parse("b/package.json", SIMPLE_PKG);
    assert_eq!(index.len(), 2);
}
