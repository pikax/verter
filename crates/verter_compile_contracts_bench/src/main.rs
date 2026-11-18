use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("compile-contract crate must live below the workspace root");
    let tests = trybuild::TestCases::new();
    tests.compile_fail(
        root.join("crates/verter_compiler/tests/cases/compile-fail")
            .join("pending_nav_request_unreachable.rs"),
    );
}
