//! External source merging and main module assembly.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use verter_core::compile::{format_import_specifier, VerterCompileResult};

use crate::id::render_ids;
use crate::types::{CompileProfile, FileMeta, HmrStrategy, SrcBlockInfo, VirtualNodeKind};

pub(crate) fn merge_external_sources(
    source: &str,
    src_blocks: &[SrcBlockInfo],
    external_sources: &FxHashMap<String, Arc<str>>,
) -> String {
    let mut merged = source.to_string();
    // Sort by descending tag_open_start so splicing from the end doesn't
    // shift earlier offsets. Use sorted indices to avoid cloning blocks.
    let mut indices: Vec<usize> = (0..src_blocks.len()).collect();
    indices.sort_by(|&a, &b| {
        src_blocks[b]
            .tag_open_start
            .cmp(&src_blocks[a].tag_open_start)
    });

    for idx in indices {
        let block = &src_blocks[idx];
        let ext = external_sources
            .get(&block.resolved_canonical_id)
            .map(|s| s.as_ref())
            .unwrap_or("");

        if let Some(close_start) = block.tag_close_start {
            merged.replace_range(block.tag_open_end as usize..close_start as usize, ext);
        } else {
            let open_raw = &merged[block.tag_open_start as usize..block.tag_open_end as usize];
            let open_fixed = if let Some(stripped) = open_raw.strip_suffix("/>") {
                format!("{}>", stripped)
            } else {
                open_raw.to_string()
            };
            let replacement = format!("{}{} </{}>", open_fixed, ext, block.tag_name);
            merged.replace_range(
                block.tag_open_start as usize..block.tag_open_end as usize,
                &replacement,
            );
        }
    }

    merged
}

pub(crate) fn assemble_main_module(
    canonical_id: &str,
    compiled: &VerterCompileResult,
    meta: &FileMeta,
    profile: &CompileProfile,
) -> String {
    use std::fmt::Write;

    // Estimate capacity: script + template + overhead
    let script_len = compiled.script.as_ref().map_or(20, |s| s.code.len());
    let template_len = compiled.template.as_ref().map_or(0, |t| t.code.len());
    let mut out = String::with_capacity(script_len + template_len + 256);

    for idx in 0..compiled.styles.len() {
        let (id, _) = render_ids(canonical_id, &VirtualNodeKind::Style { index: idx }, meta);
        let _ = writeln!(out, "import \"{}\"", id);
    }

    for idx in 0..compiled.custom_blocks.len() {
        let (id, _) = render_ids(canonical_id, &VirtualNodeKind::Custom { index: idx }, meta);
        let _ = writeln!(out, "import block{} from \"{}\"", idx, id);
    }

    if !compiled.styles.is_empty() || !compiled.custom_blocks.is_empty() {
        out.push('\n');
    }

    // Template runtime imports must come before script code (ESM requirement)
    if let Some(template) = &compiled.template {
        if !template.imports.is_empty() {
            let runtime = profile.runtime_module_name.as_deref().unwrap_or("vue");
            let _ = write!(out, "import {{ ");
            for (i, name) in template.imports.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format_import_specifier(name));
            }
            let _ = writeln!(out, " }} from \"{}\"", runtime);
        }
        // SSR helpers are imported from "vue/server-renderer"
        if !template.ssr_imports.is_empty() {
            let _ = write!(out, "import {{ ");
            for (i, name) in template.ssr_imports.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format_import_specifier(name));
            }
            let _ = writeln!(out, " }} from \"vue/server-renderer\"");
        }
    }

    if let Some(script) = &compiled.script {
        let mut script_code = script.code.clone();

        // Filter setup return to only include template-used bindings.
        // This prevents type-only imports (e.g. `import { TypeName }`) from
        // appearing as value references in the return statement, which would
        // block esbuild from eliding them and cause Rollup "not exported" errors.
        if script.setup {
            let template_code = compiled.template.as_ref().map(|t| t.code.as_str());
            filter_setup_return(&mut script_code, template_code);
        }

        script_code = script_code.replace("__sfc__", "_sfc_main");
        script_code = script_code.replace("export default _sfc_main;\n", "");
        out.push_str(&script_code);
        if !script_code.ends_with('\n') {
            out.push('\n');
        }
    } else {
        out.push_str("const _sfc_main = {}\n");
        if !compiled.scope_id.is_empty() {
            let _ = writeln!(out, "_sfc_main.__scopeId = \"{}\"", compiled.scope_id);
        }
    }

    if let Some(template) = &compiled.template {
        out.push('\n');
        out.push_str(&template.code);
        if !template.code.ends_with('\n') {
            out.push('\n');
        }
        if template.code.contains("function ssrRender(") {
            out.push_str("_sfc_main.ssrRender = ssrRender\n");
        } else if template.code.contains("function render(") {
            out.push_str("_sfc_main.render = render\n");
        }
    }

    for idx in 0..compiled.custom_blocks.len() {
        let _ = writeln!(
            out,
            "if (typeof block{} === 'function') block{}(_sfc_main)",
            idx, idx
        );
    }

    if !profile.is_production {
        let _ = writeln!(out, "_sfc_main.__file = {:?}", canonical_id);
    }

    if !profile.is_production && !profile.ssr {
        match profile.hmr_strategy {
            HmrStrategy::Vite => {
                out.push_str("/* HMR(vite) */\n");
                out.push_str("if (import.meta.hot) { import.meta.hot.accept(() => {}) }\n");
            }
            HmrStrategy::Webpack => {
                out.push_str("/* HMR(webpack) */\n");
                out.push_str("if (module.hot) { module.hot.accept(() => {}) }\n");
            }
            HmrStrategy::None => {}
        }
    }

    out.push_str("export default _sfc_main");

    out
}

/// Filter the setup function's return statement to only include bindings
/// that the template actually references via `$setup.NAME` or as template
/// refs via `ref: "NAME"`.
///
/// In `<script setup>`, all imported and declared names are placed in the
/// setup return object so the template can access them.  However, imports
/// that are only used as TypeScript types (e.g. `import { TypeName }` used
/// only in `defineProps<{ prop: TypeName }>()`) would become value
/// references in the return, preventing esbuild from eliding them.
///
/// By trimming the return to template-used names only, unused type imports
/// get elided by esbuild and Rollup can resolve them correctly.
fn filter_setup_return(script: &mut String, template_code: Option<&str>) {
    // The return statement is generated by `build_setup_wrapper_end` with format:
    //   "\nreturn { name1, name2, ... };\n"
    // followed by "\n}});\n"

    // Find the closing marker for the setup wrapper
    let wrapper_end = "\n}});\n";
    let Some(wrapper_pos) = script.find(wrapper_end) else {
        return;
    };

    // Search backward from wrapper_end for the return statement
    let before_wrapper = &script[..wrapper_pos];
    let return_marker = "\nreturn ";
    let Some(ret_line_start) = before_wrapper.rfind(return_marker) else {
        return;
    };

    // Find the end of the return statement (the semicolon + newline)
    let ret_value_start = ret_line_start + return_marker.len();
    let Some(semicolon_pos) = script[ret_value_start..wrapper_pos].find(";\n") else {
        return;
    };
    let ret_value_end = ret_value_start + semicolon_pos;

    // Extract the object literal: "{ name1, name2, ... }"
    let ret_value = &script[ret_value_start..ret_value_end];
    if !ret_value.starts_with("{ ") || !ret_value.ends_with(" }") {
        return;
    }
    let names_str = &ret_value[2..ret_value.len() - 2];
    let names: Vec<&str> = names_str.split(", ").collect();

    // Filter to names that the template uses either as:
    // 1. Setup bindings: $setup.NAME
    // 2. Template refs: ref: "NAME" (Vue binds these to $setup.NAME at runtime)
    let needed: Vec<&str> = names
        .iter()
        .filter(|name| {
            // Always keep if no template (script-only component)
            let Some(tpl) = template_code else {
                return true;
            };

            // Check for $setup.NAME pattern in the compiled template code,
            // ensuring the name is a complete identifier (not a prefix of another).
            if template_has_setup_ref(tpl, name) {
                return true;
            }

            // Check for template ref usage: ref: "NAME"
            // Vue resolves `ref: "foo"` to `$setup.foo` at runtime when using
            // <script setup>, so the binding must be in the return object.
            if template_has_string_ref(tpl, name) {
                return true;
            }

            false
        })
        .copied()
        .collect();

    // Rebuild the return statement if we filtered anything out
    if needed.len() < names.len() {
        let new_ret = if needed.is_empty() {
            "{}".to_string()
        } else {
            format!("{{ {} }}", needed.join(", "))
        };
        let replacement = format!("\nreturn {};\n", new_ret);
        script.replace_range(ret_line_start..ret_value_end + 2, &replacement);
    }
}

/// Check if the compiled template references `$setup.NAME` as a complete identifier.
fn template_has_setup_ref(tpl: &str, name: &str) -> bool {
    const PREFIX: &str = "$setup.";
    let tpl_bytes = tpl.as_bytes();
    let name_bytes = name.as_bytes();
    let mut search_from = 0;
    while let Some(pos) = tpl[search_from..].find(PREFIX) {
        let abs_prefix_end = search_from + pos + PREFIX.len();
        // Check if name matches right after "$setup."
        let candidate_end = abs_prefix_end + name_bytes.len();
        if candidate_end <= tpl_bytes.len()
            && &tpl_bytes[abs_prefix_end..candidate_end] == name_bytes
        {
            // Verify it's a complete identifier (next char is not alphanumeric/_ /$)
            match tpl_bytes.get(candidate_end) {
                None => return true,
                Some(c) if c.is_ascii_alphanumeric() || *c == b'_' || *c == b'$' => {
                    search_from = candidate_end;
                    continue;
                }
                Some(_) => return true,
            }
        } else {
            search_from = abs_prefix_end;
        }
    }
    false
}

/// Check if the compiled template uses NAME as a template ref string.
/// Matches patterns like `ref: "NAME"` or `ref: 'NAME'`.
fn template_has_string_ref(tpl: &str, name: &str) -> bool {
    const PREFIX_DQ: &str = "ref: \"";
    const PREFIX_SQ: &str = "ref: '";
    let tpl_bytes = tpl.as_bytes();
    let name_bytes = name.as_bytes();

    for (prefix, quote) in [(PREFIX_DQ, b'"'), (PREFIX_SQ, b'\'')] {
        let mut search_from = 0;
        while let Some(pos) = tpl[search_from..].find(prefix) {
            let name_start = search_from + pos + prefix.len();
            let name_end = name_start + name_bytes.len();
            if name_end < tpl_bytes.len()
                && &tpl_bytes[name_start..name_end] == name_bytes
                && tpl_bytes[name_end] == quote
            {
                return true;
            }
            search_from = name_start;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_replaces_content_between_open_close() {
        let source = "<template>old content</template>";
        let blocks = vec![SrcBlockInfo {
            tag_name: "template".to_string(),
            resolved_canonical_id: "tpl.html".to_string(),
            tag_open_start: 0,
            tag_open_end: 10,
            tag_close_start: Some(21),
        }];
        let mut ext = FxHashMap::default();
        ext.insert("tpl.html".to_string(), Arc::<str>::from("<div>new</div>"));
        let result = merge_external_sources(source, &blocks, &ext);
        assert_eq!(result, "<template><div>new</div></template>");
    }

    #[test]
    fn merge_self_closing_tag_rewrite() {
        let source = "<template src=\"./t.html\"/>";
        let blocks = vec![SrcBlockInfo {
            tag_name: "template".to_string(),
            resolved_canonical_id: "t.html".to_string(),
            tag_open_start: 0,
            tag_open_end: 26,
            tag_close_start: None,
        }];
        let mut ext = FxHashMap::default();
        ext.insert("t.html".to_string(), Arc::<str>::from("<p>hi</p>"));
        let result = merge_external_sources(source, &blocks, &ext);
        assert!(result.contains("<p>hi</p>"));
        assert!(result.contains("</template>"));
    }

    #[test]
    fn merge_multiple_blocks_correct_splice_order() {
        // Two blocks: a style at offset 50 and a template at offset 0
        // After reverse-sort, style is spliced first (higher offset), then template
        let source = "<template>tmpl</template><style>css</style>";
        let blocks = vec![
            SrcBlockInfo {
                tag_name: "template".to_string(),
                resolved_canonical_id: "t.html".to_string(),
                tag_open_start: 0,
                tag_open_end: 10,
                tag_close_start: Some(14),
            },
            SrcBlockInfo {
                tag_name: "style".to_string(),
                resolved_canonical_id: "s.css".to_string(),
                tag_open_start: 25,
                tag_open_end: 32,
                tag_close_start: Some(35),
            },
        ];
        let mut ext = FxHashMap::default();
        ext.insert("t.html".to_string(), Arc::<str>::from("<div>A</div>"));
        ext.insert("s.css".to_string(), Arc::<str>::from(".a{color:red}"));
        let result = merge_external_sources(source, &blocks, &ext);
        assert!(result.contains("<div>A</div>"));
        assert!(result.contains(".a{color:red}"));
    }

    #[test]
    fn merge_missing_source_defaults_empty() {
        let source = "<template>old</template>";
        let blocks = vec![SrcBlockInfo {
            tag_name: "template".to_string(),
            resolved_canonical_id: "missing.html".to_string(),
            tag_open_start: 0,
            tag_open_end: 10,
            tag_close_start: Some(13),
        }];
        let ext = FxHashMap::default();
        let result = merge_external_sources(source, &blocks, &ext);
        assert_eq!(result, "<template></template>");
    }

    #[test]
    fn filter_return_removes_type_only_imports() {
        let mut script = concat!(
            "import { Wallet, WalletNameMap } from '..'\n",
            "const _sfc = _defineComponent({\n",
            "  setup(__props) {\n",
            "    const props = __props;\n",
            "\nreturn { Wallet, WalletNameMap, handleClick };\n",
            "\n}});\n",
            "export default _sfc;\n",
        )
        .to_string();

        let template = "function render(_ctx, _cache, $props, $setup) {\n  \
                         return $setup.WalletNameMap + $setup.handleClick\n}";

        filter_setup_return(&mut script, Some(template));
        assert!(script.contains("return { WalletNameMap, handleClick };"));
        // Wallet should not appear in the return (but may still be in the import line)
        assert!(!script.contains("return { Wallet,"));
    }

    #[test]
    fn filter_return_keeps_all_when_all_used() {
        let mut script = concat!("\nreturn { a, b };\n", "\n}});\n",).to_string();

        let template = "$setup.a and $setup.b";
        filter_setup_return(&mut script, Some(template));
        assert!(script.contains("return { a, b };"));
    }

    #[test]
    fn filter_return_no_template_keeps_all() {
        let mut script = concat!("\nreturn { a, b };\n", "\n}});\n",).to_string();

        filter_setup_return(&mut script, None);
        assert!(script.contains("return { a, b };"));
    }

    #[test]
    fn filter_return_empty_when_none_used() {
        let mut script = concat!("\nreturn { TypeA, TypeB };\n", "\n}});\n",).to_string();

        let template = "function render() { return 'no setup refs' }";
        filter_setup_return(&mut script, Some(template));
        assert!(script.contains("return {};"));
    }

    /// Regression: template refs use `ref: "name"` (string literal) not `$setup.name`.
    /// `filter_setup_return` must preserve bindings used as template refs.
    #[test]
    fn filter_return_preserves_template_ref_bindings() {
        let mut script = concat!(
            "\nreturn { editorContainer, editor, pendingCode };\n",
            "\n}});\n",
        )
        .to_string();

        // Template has ref: "editorContainer" but no $setup.* references
        let template = r#"function render(_ctx, _cache, $props, $setup) {
  return (_openBlock(), _createElementVNode("div", { class: "editor-wrapper" }, [_createElementVNode("div", { ref: "editorContainer", class: "editor-container" }, null, 32)]))
}"#;

        filter_setup_return(&mut script, Some(template));
        // editorContainer must be kept because it's used as a template ref
        assert!(
            script.contains("editorContainer"),
            "editorContainer must be preserved for template ref binding. Got: {}",
            script
        );
    }

    /// Template ref binding should work alongside $setup references.
    #[test]
    fn filter_return_keeps_ref_and_setup_bindings() {
        let mut script =
            concat!("\nreturn { container, msg, TypeOnly };\n", "\n}});\n",).to_string();

        let template = r#"function render(_ctx, _cache, $props, $setup) {
  return (_openBlock(), _createElementVNode("div", { ref: "container" }, [_createElementVNode("span", null, $setup.msg)]))
}"#;

        filter_setup_return(&mut script, Some(template));
        assert!(
            script.contains("container"),
            "container must be kept (template ref). Got: {}",
            script
        );
        assert!(
            script.contains("msg"),
            "msg must be kept ($setup.msg). Got: {}",
            script
        );
        assert!(
            !script.contains("TypeOnly"),
            "TypeOnly should be filtered out. Got: {}",
            script
        );
    }

    // ═══════════════════════════════════════════════════════════
    // Phase 2: assemble_main_module tests
    // ═══════════════════════════════════════════════════════════

    use verter_core::compile::{
        VerterCompileResult, VerterCustomBlock, VerterScriptBlock, VerterTemplateBlock,
    };

    fn basic_compiled_result() -> VerterCompileResult {
        VerterCompileResult {
            script: Some(VerterScriptBlock {
                code: "const __sfc__ = _defineComponent({\n  setup(__props) {\n    const n = 1;\n\nreturn { n };\n\n}});\nexport default __sfc__;\n".to_string(),
                source_map: String::new(),
                setup: true,
                attrs: vec![],
                duration_ms: 0.0,
            }),
            template: Some(VerterTemplateBlock {
                code: "function render(_ctx, _cache, $props, $setup) {\n  return $setup.n\n}".to_string(),
                source_map: String::new(),
                imports: vec!["_openBlock", "_createElementBlock"],
                ssr_imports: vec![],
                duration_ms: 0.0,
                attrs: vec![],
            }),
            styles: vec![],
            custom_blocks: vec![],
            scope_id: String::new(),
            errors: vec![],
            parse_duration_ms: 0.0,
            total_duration_ms: 0.0,
            tsx: None,
            tsc: None,
            template_data: None,
        }
    }

    /// @ai-generated - SSR profile skips HMR block
    #[test]
    fn assemble_main_module_ssr_skips_hmr() {
        let compiled = basic_compiled_result();
        let profile = CompileProfile {
            is_production: false,
            ssr: true,
            hmr_strategy: HmrStrategy::Vite,
            ..CompileProfile::default()
        };
        let meta = FileMeta {
            has_script: true,
            has_template: true,
            ..FileMeta::default()
        };
        let result = assemble_main_module("Comp.vue", &compiled, &meta, &profile);
        assert!(!result.contains("import.meta.hot"));
        assert!(!result.contains("module.hot"));
    }

    /// @ai-generated - Webpack HMR strategy uses module.hot
    #[test]
    fn assemble_main_module_webpack_hmr() {
        let compiled = basic_compiled_result();
        let profile = CompileProfile {
            is_production: false,
            ssr: false,
            hmr_strategy: HmrStrategy::Webpack,
            ..CompileProfile::default()
        };
        let meta = FileMeta {
            has_script: true,
            has_template: true,
            ..FileMeta::default()
        };
        let result = assemble_main_module("Comp.vue", &compiled, &meta, &profile);
        assert!(result.contains("module.hot"));
        assert!(!result.contains("import.meta.hot"));
    }

    /// @ai-generated - No script and no template → bare `const _sfc_main = {}`
    #[test]
    fn assemble_main_module_no_script_no_template() {
        let compiled = VerterCompileResult {
            script: None,
            template: None,
            styles: vec![],
            custom_blocks: vec![],
            scope_id: String::new(),
            errors: vec![],
            parse_duration_ms: 0.0,
            total_duration_ms: 0.0,
            tsx: None,
            tsc: None,
            template_data: None,
        };
        let profile = CompileProfile::default();
        let result = assemble_main_module("Comp.vue", &compiled, &FileMeta::default(), &profile);
        assert!(result.contains("const _sfc_main = {}"));
    }

    /// @ai-generated - Custom blocks produce import + invocation lines
    #[test]
    fn assemble_main_module_custom_blocks() {
        let compiled = VerterCompileResult {
            script: None,
            template: None,
            styles: vec![],
            custom_blocks: vec![VerterCustomBlock {
                block_type: "i18n".to_string(),
                content: "{\"en\":{}}".to_string(),
                attrs: vec![],
            }],
            scope_id: String::new(),
            errors: vec![],
            parse_duration_ms: 0.0,
            total_duration_ms: 0.0,
            tsx: None,
            tsc: None,
            template_data: None,
        };
        let profile = CompileProfile::default();
        let meta = FileMeta {
            custom_types: vec!["i18n".to_string()],
            custom_langs: vec![None],
            ..FileMeta::default()
        };
        let result = assemble_main_module("Comp.vue", &compiled, &meta, &profile);
        assert!(result.contains("import block0 from"));
        assert!(result.contains("if (typeof block0 === 'function') block0(_sfc_main)"));
    }

    /// @ai-generated - Production mode skips __file
    #[test]
    fn assemble_main_module_production_skips_file() {
        let compiled = basic_compiled_result();
        let profile = CompileProfile {
            is_production: true,
            ..CompileProfile::default()
        };
        let meta = FileMeta {
            has_script: true,
            has_template: true,
            ..FileMeta::default()
        };
        let result = assemble_main_module("Comp.vue", &compiled, &meta, &profile);
        assert!(!result.contains("__file"));
    }

    /// @ai-generated - filter_setup_return preserves single name
    #[test]
    fn filter_setup_return_single_name() {
        let mut script = concat!("\nreturn { count };\n", "\n}});\n",).to_string();
        let template = "$setup.count + 1";
        filter_setup_return(&mut script, Some(template));
        assert!(script.contains("return { count };"));
    }

    /// @ai-generated - assemble_main_module with styles produces import lines
    #[test]
    fn assemble_main_module_with_styles_produces_import_lines() {
        use verter_core::compile::VerterStyleBlock;

        let compiled = VerterCompileResult {
            script: None,
            template: None,
            styles: vec![
                VerterStyleBlock {
                    code: ".a{}".to_string(),
                    scoped: false,
                    lang: None,
                    duration_ms: 0.0,
                    attrs: vec![],
                },
                VerterStyleBlock {
                    code: ".b{}".to_string(),
                    scoped: false,
                    lang: Some("scss".to_string()),
                    duration_ms: 0.0,
                    attrs: vec![],
                },
            ],
            custom_blocks: vec![],
            scope_id: String::new(),
            errors: vec![],
            parse_duration_ms: 0.0,
            total_duration_ms: 0.0,
            tsx: None,
            tsc: None,
            template_data: None,
        };
        let meta = FileMeta {
            style_langs: vec![None, Some("scss".to_string())],
            ..FileMeta::default()
        };
        let profile = CompileProfile::default();
        let result = assemble_main_module("Comp.vue", &compiled, &meta, &profile);
        assert!(
            result.contains("import \"Comp.vue?vue&type=style&index=0"),
            "should import style 0: {}",
            result
        );
        assert!(
            result.contains("import \"Comp.vue?vue&type=style&index=1"),
            "should import style 1: {}",
            result
        );
    }

    /// @ai-generated - filter_setup_return: Wallet (prefix) filtered out when only
    /// WalletNameMap is NOT used but Wallet IS used (reverse direction test)
    #[test]
    fn filter_setup_return_prefix_only_keeps_exact_match() {
        let mut script = concat!(
            "import { Wallet, WalletNameMap } from '..'\n",
            "const _sfc = _defineComponent({\n",
            "  setup(__props) {\n",
            "\nreturn { Wallet, WalletNameMap };\n",
            "\n}});\n",
        )
        .to_string();

        // Template uses $setup.Wallet (exact) but not $setup.WalletNameMap
        let template =
            "function render(_ctx, _cache, $props, $setup) {\n  return $setup.Wallet + 1\n}";
        filter_setup_return(&mut script, Some(template));
        assert!(
            script.contains("return { Wallet };"),
            "Wallet should be kept, got: {}",
            script
        );
    }

    /// @ai-generated - Vite HMR code generation in dev mode
    #[test]
    fn assemble_main_module_vite_hmr() {
        let compiled = basic_compiled_result();
        let profile = CompileProfile {
            is_production: false,
            ssr: false,
            hmr_strategy: HmrStrategy::Vite,
            ..CompileProfile::default()
        };
        let meta = FileMeta {
            has_script: true,
            has_template: true,
            ..FileMeta::default()
        };
        let result = assemble_main_module("Comp.vue", &compiled, &meta, &profile);
        assert!(
            result.contains("import.meta.hot"),
            "should contain Vite HMR code"
        );
        assert!(
            result.contains("HMR(vite)"),
            "should contain HMR(vite) comment"
        );
    }

    /// @ai-generated - Render function binding: _sfc_main.render = render
    #[test]
    fn assemble_main_module_render_function_binding() {
        let compiled = basic_compiled_result();
        let profile = CompileProfile::default();
        let meta = FileMeta {
            has_script: true,
            has_template: true,
            ..FileMeta::default()
        };
        let result = assemble_main_module("Comp.vue", &compiled, &meta, &profile);
        assert!(
            result.contains("_sfc_main.render = render"),
            "should bind render function to component"
        );
    }

    /// @ai-generated - filter_setup_return no-op when wrapper marker missing
    #[test]
    fn filter_setup_return_no_wrapper_marker() {
        let mut script = "const x = 1;\nreturn { x };\n".to_string();
        let original = script.clone();
        filter_setup_return(&mut script, Some("$setup.x"));
        assert_eq!(script, original); // unchanged — no "\n}});\n" marker
    }

    /// @ai-generated - Canary: verter_core's setup wrapper output contains the
    /// exact markers that filter_setup_return relies on. If core changes format,
    /// this test catches the mismatch.
    #[test]
    fn filter_setup_return_markers_present_in_real_compile_output() {
        use oxc_allocator::Allocator;
        use verter_core::compile::CodegenOptions;
        use verter_core::compile::{compile as compile_sfc, VerterCompileOptions};

        let source = "<script setup>\nimport { ref } from 'vue'\nconst msg = ref('hello')\n</script>\n<template><div>{{ msg }}</div></template>";
        let alloc = Allocator::new();
        let opts = CodegenOptions {
            inline: Some(false),
            ..CodegenOptions::default()
        };
        let vopts = VerterCompileOptions::default();
        let result = compile_sfc(source, &opts, &vopts, &alloc);

        let script = result.script.expect("should have script output");
        assert!(script.setup, "compiled script should be flagged as setup");

        // These are the exact markers filter_setup_return searches for.
        // If verter_core changes the wrapper format, this test will fail
        // and signal that filter_setup_return needs updating.
        assert!(
            script.code.contains("\n}});\n"),
            "script output must contain wrapper end marker '\\n}}}});\\n', got:\n{}",
            script.code
        );
        assert!(
            script.code.contains("\nreturn "),
            "script output must contain return marker '\\nreturn ', got:\n{}",
            script.code
        );
    }

    /// @ai-generated - Regression: template-only SFC must produce valid assembled output
    /// with _sfc_main defined (no script block → fallback to empty object).
    #[test]
    fn assemble_main_module_template_only_sfc() {
        use oxc_allocator::Allocator;
        use verter_core::compile::CodegenOptions;
        use verter_core::compile::{compile as compile_sfc, VerterCompileOptions};

        let source = "<template><div>hello</div></template>";
        let alloc = Allocator::new();
        let opts = CodegenOptions {
            inline: Some(false),
            ..CodegenOptions::default()
        };
        let vopts = VerterCompileOptions {
            force_js: true,
            ..Default::default()
        };
        let result = compile_sfc(source, &opts, &vopts, &alloc);

        // script should be None for template-only SFC
        assert!(
            result.script.is_none(),
            "template-only SFC should have no script block"
        );
        assert!(
            result.template.is_some(),
            "template-only SFC should have template block"
        );

        let profile = CompileProfile::default();
        let meta = FileMeta {
            has_template: true,
            ..FileMeta::default()
        };
        let assembled = assemble_main_module("NoScript.vue", &result, &meta, &profile);

        // Must contain _sfc_main definition (fallback empty object)
        assert!(
            assembled.contains("const _sfc_main = {}"),
            "template-only SFC must define _sfc_main, got:\n{}",
            assembled
        );
        // Must bind render function
        assert!(
            assembled.contains("_sfc_main.render = render"),
            "template-only SFC must bind render, got:\n{}",
            assembled
        );
        // Must export
        assert!(
            assembled.contains("export default _sfc_main"),
            "template-only SFC must export, got:\n{}",
            assembled
        );
    }

    /// @ai-generated - Multi-root template must use Fragment wrapping
    #[test]
    fn compile_multi_root_template_uses_fragment() {
        use oxc_allocator::Allocator;
        use verter_core::compile::CodegenOptions;
        use verter_core::compile::{compile as compile_sfc, VerterCompileOptions};

        let source = "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div>aaaaa</template>";
        let alloc = Allocator::new();
        let opts = CodegenOptions {
            inline: Some(false),
            ..CodegenOptions::default()
        };
        let vopts = VerterCompileOptions {
            force_js: true,
            ..Default::default()
        };
        let result = compile_sfc(source, &opts, &vopts, &alloc);

        let tpl = result.template.expect("should have template block");

        // Multi-root template must use Fragment
        assert!(
            tpl.code.contains("_Fragment"),
            "multi-root template should use _Fragment, got:\n{}",
            tpl.code
        );
        // Must include _createTextVNode for the text node
        assert!(
            tpl.code.contains("_createTextVNode"),
            "multi-root template should use _createTextVNode for text, got:\n{}",
            tpl.code
        );
        // Imports must include Fragment
        assert!(
            tpl.imports.contains(&"_Fragment"),
            "multi-root template imports must include _Fragment, got: {:?}",
            tpl.imports
        );
    }
}
