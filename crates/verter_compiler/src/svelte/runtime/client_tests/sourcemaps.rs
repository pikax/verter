use super::*;

#[test]
fn client_source_map_preserves_instance_script_rewrite_tokens() {
    let source = "<script>\nlet count = $state(0);\ncount += 1;\n</script>\n<p>{count}</p>\n";
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("Counter.svelte".to_string()),
        ..Default::default()
    };
    let alloc = Allocator::default();
    let module = compile_client(source, &parsed, &opts, &alloc, false, true)
        .expect("the script-rewrite fixture compiles");
    let map = oxc_sourcemap::OwnedSourceMap::from_json_string(
        module.source_map.as_deref().expect("demanded JS map"),
    )
    .expect("valid JS map");
    let generated_write = module
        .code
        .find("$.set(count")
        .expect("the script write is lowered")
        + "$.set(".len();
    let source_write = source
        .find("count += 1")
        .expect("the authored script write");
    assert_generated_offset_maps_to_exact_source_offset(
        &map,
        &module.code,
        generated_write,
        source_write,
    );
}
