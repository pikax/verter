//! The option path a request-construction refusal names is read off the
//! committed option inventory, not reconstructed from a Rust variant
//! spelling.
//!
//! `FrameworkOption`'s `Display` is what every transport's refusal message
//! embeds ("unsupported option '<path>'"), so the path it prints is public
//! surface: a caller reads it to find the field they wrote. Deriving it by
//! case-lowering the `Debug` spelling of the variant produces a field name
//! that exists nowhere — `TransformOptionsHoistStatic` reads
//! `vue:transformOptionsHoistStatic` while the request field is
//! `hoistStatic` — so the path comes from `VueOption::tsv_row` /
//! `SvelteOption::tsv_row`, and this file is what keeps those two rows
//! honest against the inventory files they claim to quote.
//!
//! Discriminating in both directions: `tsv_rows_match_the_committed_inventory`
//! compares the full row multiset, so a variant quoting a row the TSV does
//! not have fails, and a TSV row no variant quotes fails too.
//!
//! Mutation recipes:
//! - Change one `VueOption::tsv_row` arm's option column (e.g.
//!   `TransformOptionsHoistStatic` to `"hoisted"`):
//!   `tsv_rows_match_the_committed_inventory` reports the invented row and
//!   the unquoted one, and `refusal_paths_name_the_field_a_caller_wrote`
//!   reports the wrong path.
//! - Drop the surface-derived prefix from `FrameworkOption`'s `Display`
//!   (`write!(f, "{framework}:{option}")` unconditionally): the two
//!   `customElement` assertions in
//!   `refusal_paths_name_the_field_a_caller_wrote` go red. The nested rows
//!   would then read `svelte:tag` and `svelte:*.type`, which no caller
//!   writes. (`no_two_distinct_options_collapse_onto_one_refusal_path` does
//!   NOT catch this one: today those leaf names happen not to collide with
//!   another row, which is why the positive assertions carry the contract
//!   and the collapse check is only its companion.)
//! - Restore the case-lowered `format!("{option:?}")` rendering: every
//!   assertion in `refusal_paths_name_the_field_a_caller_wrote` goes red,
//!   and the upper-case-leading-segment check does not save it — the
//!   lowering hides the leak from a naive check, which is why the
//!   assertions are against the inventory and not against letter case.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use verter_compiler::compile_request::svelte::ALL_SVELTE_OPTIONS;
use verter_compiler::compile_request::vue::ALL_VUE_OPTIONS;
use verter_compiler::compile_request::{FrameworkOption, SvelteOption, VueOption};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is <workspace>/crates/verter_compiler")
        .to_path_buf()
}

/// The `(surface, option)` column pairs of one inventory file, in the order
/// the file lists them.
fn inventory_rows(file_name: &str) -> Vec<(String, String)> {
    let path = workspace_root()
        .join("packages/framework-conformance-harness/evidence")
        .join(file_name);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{file_name} must be readable at {path:?}: {e}"));
    raw.lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut columns = line.split('\t');
            let surface = columns
                .next()
                .unwrap_or_else(|| panic!("{file_name}: row without a surface column"));
            let option = columns
                .next()
                .unwrap_or_else(|| panic!("{file_name}: row without an option column"));
            (surface.to_string(), option.to_string())
        })
        .collect()
}

#[test]
fn tsv_rows_match_the_committed_inventory() {
    for (file_name, quoted) in [
        (
            "vue-options.tsv",
            ALL_VUE_OPTIONS
                .iter()
                .map(|option| {
                    let (surface, name) = option.tsv_row();
                    (surface.to_string(), name.to_string())
                })
                .collect::<Vec<_>>(),
        ),
        (
            "svelte-options.tsv",
            ALL_SVELTE_OPTIONS
                .iter()
                .map(|option| {
                    let (surface, name) = option.tsv_row();
                    (surface.to_string(), name.to_string())
                })
                .collect::<Vec<_>>(),
        ),
    ] {
        let committed = inventory_rows(file_name);
        assert_eq!(
            quoted.len(),
            committed.len(),
            "{file_name}: {} rows quoted by variants, {} rows committed",
            quoted.len(),
            committed.len()
        );

        let quoted_set: BTreeSet<_> = quoted.iter().cloned().collect();
        let committed_set: BTreeSet<_> = committed.iter().cloned().collect();
        assert_eq!(
            quoted_set.len(),
            quoted.len(),
            "{file_name}: two variants quote the same inventory row"
        );

        let invented: Vec<_> = quoted_set.difference(&committed_set).collect();
        assert!(
            invented.is_empty(),
            "{file_name}: variants quote rows the inventory does not have: {invented:?}"
        );
        let unquoted: Vec<_> = committed_set.difference(&quoted_set).collect();
        assert!(
            unquoted.is_empty(),
            "{file_name}: inventory rows no variant quotes: {unquoted:?}"
        );
    }
}

#[test]
fn refusal_paths_name_the_field_a_caller_wrote() {
    // A flat surface contributes no path prefix: the caller's field is the
    // bare option name, and the `Debug`-derived spelling
    // (`transformOptionsHoistStatic`) is a field that exists nowhere.
    assert_eq!(
        FrameworkOption::Vue(VueOption::TransformOptionsHoistStatic).to_string(),
        "vue:hoistStatic"
    );
    assert_eq!(
        FrameworkOption::Vue(VueOption::ParserOptionsCompatConfig).to_string(),
        "vue:compatConfig"
    );
    // A nested option keeps the nesting the inventory records for it.
    assert_eq!(
        FrameworkOption::Vue(VueOption::ParserOptionsCompatConfigMode).to_string(),
        "vue:compatConfig.MODE"
    );
    assert_eq!(
        FrameworkOption::Svelte(SvelteOption::CompileOptionsAccessors).to_string(),
        "svelte:accessors"
    );
    // A surface whose own column carries option-path segments contributes
    // them, so the two `customElement` surfaces do not collapse onto their
    // bare leaf names.
    assert_eq!(
        FrameworkOption::Svelte(SvelteOption::CustomElementTag).to_string(),
        "svelte:customElement.tag"
    );
    assert_eq!(
        FrameworkOption::Svelte(SvelteOption::CustomElementPropsType).to_string(),
        "svelte:customElement.props.*.type"
    );
}

#[test]
fn no_two_distinct_options_collapse_onto_one_refusal_path() {
    // An option name that recurs across inventory surfaces (`filename`,
    // `sourceMap`, `hoistStatic`, …) is ONE field on the framework's
    // request object, so sharing a path is correct. Two DIFFERENT option
    // names sharing one path would not be: the refusal would name a field
    // the caller did not write. That is what the surface-derived prefix is
    // for, and what this asserts.
    let mut paths: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
    for option in ALL_VUE_OPTIONS
        .iter()
        .map(|o| FrameworkOption::Vue(*o))
        .chain(
            ALL_SVELTE_OPTIONS
                .iter()
                .map(|o| FrameworkOption::Svelte(*o)),
        )
    {
        let path = option.to_string();
        let (framework, tail) = path
            .split_once(':')
            .unwrap_or_else(|| panic!("{path} is not framework-tagged"));
        assert_eq!(framework, option.framework(), "{path}");
        assert!(!tail.is_empty(), "{path} has an empty option path");
        assert!(
            !tail.chars().next().is_some_and(char::is_uppercase),
            "{path} leads with an upper-case segment, which is a leaked Rust variant spelling"
        );
        paths.entry(path).or_default().insert(option.tsv_row().1);
    }

    let collapsed: Vec<_> = paths
        .iter()
        .filter(|(_, options)| options.len() > 1)
        .collect();
    assert!(
        collapsed.is_empty(),
        "distinct inventory options share one refusal path: {collapsed:?}"
    );
    assert_eq!(
        paths.len(),
        123,
        "the 153 inventory rows name 123 distinct caller fields"
    );
}
