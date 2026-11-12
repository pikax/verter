fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc should exist");
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config.skip_source_info();
    // The node-reference rejections echo a whole `NodeRef` (four digests).
    // Boxed, they keep `TypeInfoRequestError` as small as its other arms:
    // the recursive request validators hold one in every frame.
    config.boxed(".verter.v1.TypeInfoRequestError.kind.stale_basis");
    config.boxed(".verter.v1.TypeInfoRequestError.kind.unknown_node_ref");
    config.file_descriptor_set_path(
        std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"))
            .join("component_meta_descriptor.bin"),
    );
    config
        .compile_protos(
            &[
                "proto/verter/v1/compiler_intrinsics.proto",
                "proto/verter/v1/component_meta.proto",
                "proto/verter/v1/selective_component_meta.proto",
                "proto/verter/v1/typeinfo.proto",
            ],
            &["proto"],
        )
        .expect("component-meta proto should compile");
}
