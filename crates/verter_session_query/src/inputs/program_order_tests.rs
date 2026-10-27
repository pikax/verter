use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{
    library_file_name, library_priority, needs_program_walk, orders_across_files, ProgramFileOrder,
};
use crate::resolution::membership::directory_walk_order;
use crate::resolution::{CompiledGlob, NormalizedGlob, StaticMembershipSpec};
use verter_span::path::CanonicalPath;

fn order(roots: &[&str], references: &[(&str, &[&str])]) -> Vec<String> {
    let graph: FxHashMap<&str, Vec<Arc<str>>> = references
        .iter()
        .map(|(file, refs)| (*file, refs.iter().map(|r| Arc::from(*r)).collect()))
        .collect();
    let built = ProgramFileOrder::sequence(
        Arc::from("/p/tsconfig.json"),
        roots.iter().map(|root| Arc::from(*root)),
        |file| graph.get(file).cloned(),
    );
    let mut files: Vec<(u32, String)> = graph
        .keys()
        .map(|file| (built.rank(file), (*file).to_owned()))
        .filter(|(rank, _)| *rank != u32::MAX)
        .collect();
    files.sort();
    files.into_iter().map(|(_, file)| file).collect()
}

/// A file lands after the files it references, in reference order; the
/// first visit places a file, so a later reference neither moves it nor
/// loops; a file the program cannot load takes no place.
#[test]
fn files_land_after_what_they_reference_first_visit_wins() {
    assert_eq!(
        order(
            &["a", "b", "c", "missing"],
            &[
                ("a", &["x", "y"]),
                ("x", &["y", "a"]),
                ("y", &[]),
                ("b", &["y", "z"]),
                ("z", &["gone"]),
                ("c", &[]),
            ],
        ),
        ["y", "x", "a", "z", "b", "c"]
    );
}

/// A reference chain deeper than any thread stack still sequences.
#[test]
fn deep_reference_chains_do_not_recurse() {
    let depth = 200_000usize;
    let built = ProgramFileOrder::sequence(Arc::from("/p"), [Arc::from("0")], |file| {
        let at: usize = file.parse().expect("numbered file");
        Some(if at + 1 < depth {
            vec![Arc::from((at + 1).to_string())]
        } else {
            Vec::new()
        })
    });
    assert_eq!(built.rank(&(depth - 1).to_string()), 0);
    assert_eq!(built.rank("0"), (depth - 1) as u32);
}

/// The fingerprint names the sequence: a reordering moves it, the same
/// sequence reached through other references does not.
#[test]
fn fingerprint_moves_exactly_with_the_sequence() {
    let program: Arc<str> = Arc::from("/p/tsconfig.json");
    let build = |roots: &[&str], refs: &[(&str, &str)]| {
        let refs: FxHashMap<&str, &str> = refs.iter().copied().collect();
        ProgramFileOrder::sequence(
            Arc::clone(&program),
            roots.iter().map(|root| Arc::from(*root)),
            |file| {
                Some(
                    refs.get(file)
                        .map(|r| vec![Arc::from(*r)])
                        .unwrap_or_default(),
                )
            },
        )
    };
    let listed = build(&["a", "b"], &[]);
    let imported = build(&["b"], &[("b", "a")]);
    let swapped = build(&["b", "a"], &[]);
    assert_eq!(listed.fingerprint(), imported.fingerprint());
    assert_ne!(listed.fingerprint(), swapped.fingerprint());
    assert!(listed.fact().is_some());
    assert!(ProgramFileOrder::unordered().fact().is_none());
}

#[test]
fn only_two_distinct_program_files_need_an_order() {
    assert!(!orders_across_files(["/a.ts", "/a.ts"]));
    assert!(!orders_across_files(["/a.ts", "ambient:/lib.d.ts"]));
    assert!(orders_across_files(["/a.ts", "/b.ts"]));
}

/// The library files a walk reaches precede every program file, by
/// TypeScript's library priority — its library table's order, `lib.d.ts`
/// first, an unknown library last — and, among equals, by visit order;
/// a file's reference to a library does not place it after that file.
///
/// Measured on the pinned TypeScript (re-measured on 7.1.0-beta) with
/// `lib: ["es5"]` and a root file referencing `es2015`: the program lists
/// `lib.es5.d.ts`,
/// `lib.es2015.d.ts`, `lib.es2015.core.d.ts`,
/// `lib.es2015.collection.d.ts`, then the root — although
/// `lib.es2015.d.ts` references `es5` first and is reached first.
#[test]
fn libraries_precede_program_files_by_priority() {
    let es5 = "ambient:/t/lib.es5.d.ts";
    let es2015 = "ambient:/t/lib.es2015.d.ts";
    let core = "ambient:/t/lib.es2015.core.d.ts";
    let collection = "ambient:/t/lib.es2015.collection.d.ts";
    let custom_b = "ambient:/t/lib.b.d.ts";
    let custom_a = "ambient:/t/lib.a.d.ts";
    let full = "ambient:/t/lib.d.ts";
    let graph: FxHashMap<&str, Vec<Arc<str>>> = [
        ("/p/main.ts", vec![Arc::from(es2015)]),
        (
            es2015,
            vec![Arc::from(collection), Arc::from(es5), Arc::from(core)],
        ),
        (es5, vec![]),
        (core, vec![]),
        (collection, vec![]),
        (custom_b, vec![]),
        (custom_a, vec![]),
        (full, vec![]),
    ]
    .into_iter()
    .collect();
    let built = ProgramFileOrder::sequence(
        Arc::from("/p/tsconfig.json"),
        ["/p/main.ts", custom_b, es5, custom_a, full]
            .into_iter()
            .map(Arc::from),
        |file| graph.get(file).cloned(),
    );
    let mut listed: Vec<(u32, &str)> = graph.keys().map(|file| (built.rank(file), *file)).collect();
    listed.sort();
    assert_eq!(
        listed.into_iter().map(|(_, file)| file).collect::<Vec<_>>(),
        [
            full,
            es5,
            es2015,
            core,
            collection,
            custom_b,
            custom_a,
            "/p/main.ts"
        ]
    );
    assert!(built.loads_library(core));
    assert!(!built.loads_library("ambient:/t/lib.dom.d.ts"));
    assert!(ProgramFileOrder::unordered().loads_library(core));
}

/// A `/// <reference lib>` name loads the file TypeScript's library table
/// maps it to, case-insensitively; an unknown name loads nothing.
#[test]
fn library_references_name_the_table_s_files() {
    assert_eq!(
        library_file_name("ES2015.Core"),
        Some("lib.es2015.core.d.ts")
    );
    assert_eq!(library_file_name("es6"), Some("lib.es2015.d.ts"));
    assert_eq!(
        library_file_name("esnext.string"),
        Some("lib.es2024.string.d.ts")
    );
    assert_eq!(library_file_name("es2015.full"), None);
    assert!(library_priority("lib.d.ts") < library_priority("lib.es5.d.ts"));
    assert!(library_priority("lib.es5.d.ts") < library_priority("lib.es2015.core.d.ts"));
    assert!(library_priority("lib.decorators.legacy.d.ts") < library_priority("lib.custom.d.ts"));
}

/// Only two program files, or a library the libraries alone do not reach,
/// need the program's whole order.
#[test]
fn a_walk_is_needed_for_two_program_files_or_an_unreached_library() {
    let es5 = "ambient:/t/lib.es5.d.ts";
    let core = "ambient:/t/lib.es2015.core.d.ts";
    let libraries = ProgramFileOrder::libraries([Arc::from(es5)], |_| Some(Vec::new()));
    assert!(!needs_program_walk(["/a.ts", es5], &libraries));
    assert!(needs_program_walk(["/a.ts", core], &libraries));
    assert!(needs_program_walk(["/a.ts", "/b.ts"], &libraries));
    assert!(libraries.fact().is_none());
}

/// TypeScript's `matchFiles` lists a directory's files before its
/// subdirectories, each in ordinal order, depth first — not path order.
#[test]
fn directory_walk_lists_files_before_subdirectories() {
    let mut paths = vec![
        "/p/src/z.ts",
        "/p/src/b/x.ts",
        "/p/src/a.ts",
        "/p/src/sub/m.ts",
        "/p/src/B.ts",
    ];
    paths.sort_by(|l, r| directory_walk_order(l, r));
    assert_eq!(
        paths,
        [
            "/p/src/B.ts",
            "/p/src/a.ts",
            "/p/src/z.ts",
            "/p/src/b/x.ts",
            "/p/src/sub/m.ts"
        ]
    );
}

/// Root files: `files` as listed, then include matches grouped by authored
/// include entry in walk order; exclude filters only include matches; a
/// higher-priority extension shadows a same-stem wildcard match.
#[test]
fn program_roots_follow_files_then_include_entries_in_walk_order() {
    let include = |entry: u32, pattern: &str| {
        CompiledGlob::new(NormalizedGlob::new(pattern)).in_include_entry(entry)
    };
    let spec = StaticMembershipSpec {
        files: vec![
            CanonicalPath::new("/p/listed.ts"),
            CanonicalPath::new("/p/app/excluded.ts"),
        ],
        include: vec![
            include(0, "/p/lib/**/*.ts"),
            include(0, "/p/lib/**/*.d.ts"),
            include(1, "/p/app/**/*.ts"),
            include(1, "/p/app/**/*.d.ts"),
        ],
        exclude: vec![CompiledGlob::new(NormalizedGlob::new("/p/app/excluded.ts"))].into(),
    };
    let candidates: Vec<CanonicalPath> = [
        "/p/app/z.ts",
        "/p/app/excluded.ts",
        "/p/app/a/inner.ts",
        "/p/lib/types.d.ts",
        "/p/lib/types.ts",
        "/p/lib/only.d.ts",
        "/p/listed.ts",
        "/p/elsewhere.ts",
        "/p/app/z.ts",
    ]
    .into_iter()
    .map(CanonicalPath::new)
    .collect();
    let roots: Vec<String> = spec
        .program_root_files(&candidates)
        .into_iter()
        .map(|path| path.as_str().to_owned())
        .collect();
    assert_eq!(
        roots,
        [
            "/p/listed.ts",
            "/p/app/excluded.ts",
            "/p/lib/only.d.ts",
            "/p/lib/types.ts",
            "/p/app/z.ts",
            "/p/app/a/inner.ts",
        ]
    );
}

/// A `.d.ts` include match does not shadow a same-stem `.js` one: both
/// stay roots (`hasFileWithHigherPriorityExtension` skips `.d.ts` for a
/// JavaScript file), while a `.ts` one shadows both.
#[test]
fn a_declaration_file_does_not_shadow_its_javascript_sibling() {
    let spec = StaticMembershipSpec {
        files: Vec::new(),
        include: vec![CompiledGlob::new(NormalizedGlob::new("/p/**/*")).in_include_entry(0)],
        exclude: Vec::new().into(),
    };
    let roots = |paths: &[&str]| -> Vec<String> {
        let candidates: Vec<CanonicalPath> =
            paths.iter().map(|path| CanonicalPath::new(path)).collect();
        spec.program_root_files(&candidates)
            .into_iter()
            .map(|path| path.as_str().to_owned())
            .collect()
    };
    assert_eq!(roots(&["/p/a.js", "/p/a.d.ts"]), ["/p/a.d.ts", "/p/a.js"]);
    assert_eq!(roots(&["/p/a.jsx", "/p/a.d.ts"]), ["/p/a.d.ts", "/p/a.jsx"]);
    assert_eq!(roots(&["/p/a.js", "/p/a.d.ts", "/p/a.ts"]), ["/p/a.ts"]);
}
