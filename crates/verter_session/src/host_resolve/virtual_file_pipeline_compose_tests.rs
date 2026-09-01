use super::*;
use verter_compiler::framework_common::{
    RuntimeOutputDescriptor, SourceMapFidelity, TemplateRenderExport,
};

fn template(code: &str, source_map: &str, imports: Vec<String>) -> RuntimeTemplateBlock {
    RuntimeTemplateBlock {
        code: code.to_string(),
        source_map: source_map.to_string(),
        imports,
        ssr_imports: Vec::new(),
        render_export: TemplateRenderExport::Render,
        output_descriptor: RuntimeOutputDescriptor::generated(
            code,
            None,
            &[("test:space", "test:artifact")],
            SourceMapFidelity::Approximate,
        ),
    }
}

#[test]
fn no_imports_returns_the_template_verbatim() {
    let block = template("const a = 1", "", vec![]);
    let (code, map) =
        compose_template_virtual_file(&block, None).expect("no-import template composes trivially");
    assert_eq!(code, "const a = 1");
    assert!(map.is_none());
}

#[test]
fn imports_present_prepends_the_import_line_and_shifts_the_map() {
    let map_json = "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
    let block = template("const n = 1", map_json, vec!["_openBlock".to_string()]);
    let (code, map) =
        compose_template_virtual_file(&block, None).expect("import template composes");
    assert_eq!(
        code, "import { openBlock as _openBlock } from \"vue\"\nconst n = 1",
        "the import preamble must precede the template's own code verbatim"
    );
    let map = map.expect("a present input map must still be present after composition");
    let decoded = verter_compiler::oxc_sourcemap::SourceMap::from_json_string(&map).unwrap();
    let token = decoded
        .get_tokens()
        .next()
        .expect("the shifted segment survives composition");
    assert_eq!(
        token.get_dst_line(),
        1,
        "the segment must move down by exactly the one-line preamble"
    );
    assert_eq!(token.get_dst_col(), 6);
    assert_eq!(
        decoded.get_source(token.get_source_id().unwrap()),
        Some("Comp.vue"),
        "the original source identity must survive — never a synthetic placeholder"
    );
}

#[test]
fn custom_runtime_module_name_reaches_the_import_specifier() {
    let block = template("const n = 1", "", vec!["_openBlock".to_string()]);
    let (code, _) =
        compose_template_virtual_file(&block, Some("@vue/runtime-dom")).expect("composes");
    assert!(code.starts_with("import { openBlock as _openBlock } from \"@vue/runtime-dom\"\n"));
}
