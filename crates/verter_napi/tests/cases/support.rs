//! Helpers the Node-driven cases share.

use std::path::PathBuf;

/// The fixture addon's shared library, as its platform names it.
#[cfg(target_os = "windows")]
const FIXTURE_LIBRARY: &str = "verter_napi_request_fixture.dll";
#[cfg(target_os = "macos")]
const FIXTURE_LIBRARY: &str = "libverter_napi_request_fixture.dylib";
#[cfg(all(unix, not(target_os = "macos")))]
const FIXTURE_LIBRARY: &str = "libverter_napi_request_fixture.so";

/// Answers the fixture addon built beside this test binary.
///
/// An absent addon fails here, loudly and by name. It is never rebuilt: a
/// suite that can quietly produce its own input can also quietly produce the
/// wrong one.
pub(crate) fn fixture_addon_library() -> PathBuf {
    let exe = std::env::current_exe().expect("a running test has a path");
    let library = exe
        .parent()
        .expect("a test binary sits in a directory")
        .join(FIXTURE_LIBRARY);
    assert!(
        library.exists(),
        "the fixture addon must sit beside this test binary, at {}. \
         `verter_napi` dev-depends on it so cargo builds it with this \
         binary; a run from a nextest archive additionally needs the \
         `archive.include` entry naming it in `.config/nextest.toml`.",
        library.display()
    );
    library
}
