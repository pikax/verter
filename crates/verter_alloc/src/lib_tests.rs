use super::*;

#[cfg(all(not(target_family = "wasm"), not(feature = "system-allocator")))]
mod mimalloc_build {
    use std::alloc::{GlobalAlloc, Layout};
    use std::path::PathBuf;

    use super::*;

    fn vendored_header() -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/libmimalloc-sys2/c_src/mimalloc3/include/mimalloc.h");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
    }

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
    fn the_purge_delay_option_index_matches_the_vendored_enum() {
        let header = vendored_header();
        let start = header
            .find("typedef enum mi_option_e {")
            .expect("the vendored mimalloc.h declares mi_option_e");
        let body = &header[start..];
        let body = &body[body.find('{').unwrap() + 1..body.find("} mi_option_t;").unwrap()];
        let index = body
            .lines()
            .map(|line| line.split("//").next().unwrap().trim())
            .filter(|line| !line.is_empty())
            .map(|line| line.trim_end_matches(','))
            .position(|name| name == "mi_option_purge_delay")
            .expect("mi_option_e has mi_option_purge_delay");
        assert_eq!(
            libmimalloc_sys2::mi_option_t::try_from(index).unwrap(),
            selected::MI_OPTION_PURGE_DELAY
        );
    }

    #[test]
    fn configure_sets_the_purge_delay_unless_the_environment_does() {
        configure();
        let expected = std::env::var("MIMALLOC_PURGE_DELAY")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(i64::from(PURGE_DELAY_MS));
        assert_eq!(purge_delay_ms(), Some(expected));
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
            assert!(selected::owns(block));
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
