use super::*;

/// `:class` with merged static+dynamic class — source map tokens preserved via patch-based.
#[test]
fn merged_class_binding_source_map_accuracy() {
    let source = r#"<template><div class="base" :class="{ 'active': isActive }"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("isActive", BindingType::Props)]);

    // Positive: should use normalizeClass with merged static value and __props prefix
    assert!(
        output.contains("___VERTER___normalizeClass"),
        "merged class should use normalizeClass: {output}"
    );
    assert!(
        output.contains("__props.isActive"),
        "should apply __props prefix to isActive: {output}"
    );

    // Negative: no raw :class
    assert!(
        !output.contains(":class"),
        ":class directive must be removed from JSX: {output}"
    );

    // Source map: `isActive` identifier should have a token at its original source position
    let src_col = source.find("isActive").expect("isActive in source") as u32;
    let has_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == src_col);
    assert!(
        has_token,
        "source map must have a token mapping to the original isActive position (col {}), \
         but only found source columns: {:?}",
        src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}
