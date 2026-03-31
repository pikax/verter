fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc should exist");
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config.skip_source_info();
    config.file_descriptor_set_path(
        std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"))
            .join("component_meta_descriptor.bin"),
    );
    config
        .compile_protos(&["proto/verter/v1/component_meta.proto"], &["proto"])
        .expect("component-meta proto should compile");
}
