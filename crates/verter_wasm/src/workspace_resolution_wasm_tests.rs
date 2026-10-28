//! The synchronous workspace facade must drain runnable owned slices on a
//! single-thread WASM host, where a native thread cannot park between slices.
use super::host;
use std::sync::Arc;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn workspace_resolution_drains_owned_slices_without_native_parking() {
    let host = host::VerterHost::new_standalone(host::HostConfig::default());
    for (path, source) in [
        ("/p/dep.ts", "export const n = 1"),
        ("/p/main.ts", "export { n } from './dep'"),
    ] {
        let _ = host
            .upsert(host::UpsertRequest {
                canonical_id: Some(path.into()),
                input_id: path.into(),
                source: Arc::from(source),
                file_language: host::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("the in-memory source is accepted");
    }
    for _ in 0..2 {
        assert_eq!(
            host.resolve_import_transient("/p/main.ts", "./dep")
                .as_deref(),
            Some("/p/dep.ts")
        );
    }
}
