use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{orders_across_files, ProgramFileOrder};
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
