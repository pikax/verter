use super::*;

#[cfg(all(not(target_family = "wasm"), not(feature = "system-allocator")))]
mod mimalloc_build {
    use std::alloc::{GlobalAlloc, Layout};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use libmimalloc_sys2::mi_option_t;

    use super::*;

    fn vendored(relative: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/libmimalloc-sys2")
            .join(relative);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
    }

    fn vendored_header() -> String {
        vendored("c_src/mimalloc3/include/mimalloc.h")
    }

    /// Every name the vendored header's `mi_option_e` declares, with its
    /// value: the enumerators by position, and the legacy names as the value
    /// of the enumerator they alias.
    fn header_options() -> BTreeMap<String, mi_option_t> {
        let header = vendored_header();
        let start = header
            .find("typedef enum mi_option_e {")
            .expect("the vendored mimalloc.h declares mi_option_e");
        let body = &header[start..];
        let body = &body[body.find('{').unwrap() + 1..body.find("} mi_option_t;").unwrap()];
        let mut options = BTreeMap::new();
        let mut next: mi_option_t = 0;
        for line in body.lines() {
            let decl = line
                .split("//")
                .next()
                .unwrap()
                .trim()
                .trim_end_matches(',');
            if decl.is_empty() {
                continue;
            }
            match decl.split_once('=') {
                Some((alias, target)) => {
                    let value = options[target.trim()];
                    options.insert(alias.trim().to_owned(), value);
                }
                None => {
                    options.insert(decl.to_owned(), next);
                    next += 1;
                }
            }
        }
        options
    }

    macro_rules! exported_options {
        ($($name:ident),* $(,)?) => {
            [$((stringify!($name), libmimalloc_sys2::$name)),*]
        };
    }

    /// Every option constant the vendored sys crate exports, read through the
    /// compiled constants themselves.
    const EXPORTED: [(&str, mi_option_t); 54] = exported_options![
        mi_option_show_errors,
        mi_option_show_stats,
        mi_option_verbose,
        mi_option_deprecated_eager_commit,
        mi_option_arena_eager_commit,
        mi_option_purge_decommits,
        mi_option_allow_large_os_pages,
        mi_option_reserve_huge_os_pages,
        mi_option_reserve_huge_os_pages_at,
        mi_option_reserve_os_memory,
        mi_option_deprecated_segment_cache,
        mi_option_deprecated_page_reset,
        mi_option_deprecated_abandoned_page_purge,
        mi_option_deprecated_segment_reset,
        mi_option_deprecated_eager_commit_delay,
        mi_option_purge_delay,
        mi_option_use_numa_nodes,
        mi_option_disallow_os_alloc,
        mi_option_os_tag,
        mi_option_max_errors,
        mi_option_max_warnings,
        mi_option_deprecated_max_segment_reclaim,
        mi_option_destroy_on_exit,
        mi_option_arena_reserve,
        mi_option_arena_purge_mult,
        mi_option_deprecated_purge_extend_delay,
        mi_option_disallow_arena_alloc,
        mi_option_retry_on_oom,
        mi_option_deprecated_visit_abandoned,
        mi_option_guarded_min,
        mi_option_guarded_max,
        mi_option_guarded_precise,
        mi_option_guarded_sample_rate,
        mi_option_guarded_sample_seed,
        mi_option_generic_collect,
        mi_option_page_reclaim_on_free,
        mi_option_page_full_retain,
        mi_option_page_max_candidates,
        mi_option_max_vabits,
        mi_option_pagemap_commit,
        mi_option_page_commit_on_demand,
        mi_option_page_max_reclaim,
        mi_option_page_cross_thread_max_reclaim,
        mi_option_allow_thp,
        mi_option_minimal_purge_size,
        mi_option_arena_max_object_size,
        mi_option_arena_is_numa_local,
        mi_option_collect_merges_stats,
        _mi_option_last,
        mi_option_large_os_pages,
        mi_option_eager_region_commit,
        mi_option_reset_decommits,
        mi_option_reset_delay,
        mi_option_limit_os_alloc,
    ];

    #[test]
    fn the_linked_c_allocator_is_the_pinned_version() {
        assert_eq!(linked_mimalloc_version(), Some(MIMALLOC_VERSION));
    }

    #[test]
    fn the_pinned_version_is_the_vendored_one() {
        let header = vendored_header();
        let define = header
            .lines()
            .find_map(|line| line.trim().strip_prefix("#define MI_MALLOC_VERSION"))
            .expect("the vendored mimalloc.h defines MI_MALLOC_VERSION");
        let version: i32 = define
            .split_whitespace()
            .next()
            .and_then(|token| token.parse().ok())
            .expect("MI_MALLOC_VERSION is an integer literal");
        assert_eq!(version, MIMALLOC_VERSION);
    }

    #[test]
    fn every_exported_option_constant_has_its_header_value() {
        let header = header_options();
        for (name, value) in EXPORTED {
            assert_eq!(
                header.get(name),
                Some(&value),
                "`{name}` is {value} in the sys crate, but the vendored header gives {:?}",
                header.get(name)
            );
        }
        // And nothing the header declares is missing from the bindings.
        let exported: Vec<&str> = EXPORTED.iter().map(|(name, _)| *name).collect();
        for name in header.keys() {
            assert!(
                exported.contains(&name.as_str()),
                "the vendored header declares `{name}`, which the sys crate does not export"
            );
        }
    }

    #[test]
    fn the_checked_list_is_every_option_constant_the_sys_crate_exports() {
        let source = vendored("src/extended.rs");
        let mut declared: Vec<&str> = source
            .lines()
            .filter_map(|line| line.strip_prefix("pub const "))
            .filter_map(|rest| rest.strip_suffix(";"))
            .filter(|rest| rest.contains(": mi_option_t ="))
            .map(|rest| rest.split(':').next().unwrap())
            .collect();
        declared.sort_unstable();
        let mut checked: Vec<&str> = EXPORTED.iter().map(|(name, _)| *name).collect();
        checked.sort_unstable();
        assert_eq!(declared, checked);
    }

    #[test]
    fn configure_sets_the_purge_delay_unless_the_environment_did() {
        // mimalloc read its environment when it initialised, before this test
        // ran, and an option it read from there stays in force. Compare what
        // it reports before and after, rather than re-parsing the variables.
        let from_environment = ["MIMALLOC_PURGE_DELAY", "MIMALLOC_RESET_DELAY"]
            .iter()
            .any(|name| std::env::var_os(name).is_some());
        let before = purge_delay_ms();
        configure();
        let after = purge_delay_ms();
        if from_environment {
            assert_eq!(
                after, before,
                "an environment purge delay must stay in force"
            );
        } else {
            assert_eq!(after, Some(i64::from(PURGE_DELAY_MS)));
        }
    }

    #[test]
    fn the_ownership_probe_tells_mimalloc_blocks_from_system_blocks() {
        // This test binary installs no global allocator, so the global heap
        // is the platform's, and the probe must not claim it.
        assert!(!mimalloc_serves_global_allocations());

        let layout = Layout::new::<u64>();
        // SAFETY: a valid non-zero-size layout, freed below with the same
        // allocator and layout.
        unsafe {
            let block = ALLOCATOR.alloc(layout);
            assert!(!block.is_null());
            assert!(mimalloc_owns(block));
            ALLOCATOR.dealloc(block, layout);
        }
    }

    #[test]
    fn a_native_build_selects_mimalloc() {
        assert_eq!(SELECTED, AllocatorKind::Mimalloc);
    }
}

#[cfg(any(target_family = "wasm", feature = "system-allocator"))]
#[test]
fn a_system_build_selects_the_platform_heap_and_links_no_mimalloc() {
    assert_eq!(SELECTED, AllocatorKind::System);
    assert_eq!(linked_mimalloc_version(), None);
    assert_eq!(purge_delay_ms(), None);
    assert!(!mimalloc_serves_global_allocations());
}
